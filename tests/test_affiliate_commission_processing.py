from __future__ import annotations

import contextlib
import sqlite3
import unittest
from types import SimpleNamespace

from bot.dashboard.affiliate_mixin import _DashboardAffiliateMixin


class _NoRowsResult:
    def fetchone(self):
        return None

    def fetchall(self):
        return []


class _RecordingSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn
        self.lock_calls: list[tuple[str, tuple[int, int]]] = []
        self.transaction_events: list[str] = []

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").strip()
        sql_params = tuple(params or ())
        if sql_text.startswith("SELECT pg_advisory_lock"):
            self.lock_calls.append(("lock", sql_params))
            return _NoRowsResult()
        if sql_text.startswith("SELECT pg_advisory_unlock"):
            self.lock_calls.append(("unlock", sql_params))
            return _NoRowsResult()
        return self._conn.execute(str(sql or "").replace("%s", "?"), sql_params)

    def commit(self) -> None:
        self._conn.commit()

    def rollback(self) -> None:
        self._conn.rollback()

    @contextlib.contextmanager
    def transaction(self):
        self.transaction_events.append("begin")
        self._conn.execute("BEGIN IMMEDIATE")
        try:
            yield
        except Exception:
            self._conn.rollback()
            self.transaction_events.append("rollback")
            raise
        else:
            self._conn.commit()
            self.transaction_events.append("commit")

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _TransferApi:
    def __init__(self, outcomes: list[Exception | str]) -> None:
        self._outcomes = list(outcomes)
        self.calls: list[dict[str, object]] = []

    def create(self, **kwargs):
        self.calls.append(dict(kwargs))
        outcome = self._outcomes.pop(0)
        if isinstance(outcome, Exception):
            raise outcome
        return SimpleNamespace(id=outcome)


class _FakeStripe:
    def __init__(self, outcomes: list[Exception | str]) -> None:
        self.Transfer = _TransferApi(outcomes)


class _AffiliateCommissionHarness(_DashboardAffiliateMixin):
    @staticmethod
    def _affiliate_ensure_tables(_conn) -> None:
        return None


class AffiliateCommissionProcessingTests(unittest.TestCase):
    def setUp(self) -> None:
        raw_conn = sqlite3.connect(":memory:", isolation_level=None)
        raw_conn.row_factory = sqlite3.Row
        raw_conn.executescript(
            """
            CREATE TABLE affiliate_accounts (
                twitch_login TEXT PRIMARY KEY,
                stripe_account_id TEXT
            );
            CREATE TABLE affiliate_streamer_claims (
                affiliate_twitch_login TEXT NOT NULL,
                claimed_streamer_login TEXT NOT NULL UNIQUE
            );
            CREATE TABLE affiliate_commissions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                affiliate_twitch_login TEXT NOT NULL,
                streamer_login TEXT NOT NULL,
                stripe_event_id TEXT NOT NULL UNIQUE,
                stripe_invoice_id TEXT,
                stripe_customer_id TEXT,
                stripe_transfer_id TEXT,
                brutto_cents INTEGER NOT NULL,
                commission_cents INTEGER NOT NULL,
                currency TEXT NOT NULL,
                status TEXT NOT NULL,
                period_start TEXT,
                period_end TEXT,
                created_at TEXT NOT NULL,
                transferred_at TEXT,
                error_message TEXT
            );
            CREATE TABLE twitch_billing_subscriptions (
                stripe_customer_id TEXT PRIMARY KEY,
                twitch_login TEXT NOT NULL
            );
            """
        )
        self.raw_conn = raw_conn
        self.conn = _RecordingSqliteConn(raw_conn)
        self.handler = _AffiliateCommissionHarness()

    def tearDown(self) -> None:
        self.raw_conn.close()

    def _insert_affiliate_fixture(self, *, stripe_account_id: str | None) -> None:
        self.raw_conn.execute(
            "INSERT INTO affiliate_accounts (twitch_login, stripe_account_id) VALUES (?, ?)",
            ("affiliate_one", stripe_account_id),
        )
        self.raw_conn.execute(
            """INSERT INTO affiliate_streamer_claims
               (affiliate_twitch_login, claimed_streamer_login)
               VALUES (?, ?)""",
            ("affiliate_one", "streamer_one"),
        )
        self.raw_conn.execute(
            """INSERT INTO twitch_billing_subscriptions
               (stripe_customer_id, twitch_login)
               VALUES (?, ?)""",
            ("cus_123", "streamer_one"),
        )

    def test_cap_enforcement_without_stripe_uses_serialization_and_marks_skipped(self) -> None:
        self._insert_affiliate_fixture(stripe_account_id=None)
        self.raw_conn.execute(
            """
            INSERT INTO affiliate_commissions (
                affiliate_twitch_login, streamer_login, stripe_event_id, stripe_invoice_id,
                stripe_customer_id, stripe_transfer_id, brutto_cents, commission_cents,
                currency, status, period_start, period_end, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "affiliate_one",
                "streamer_one",
                "evt_existing",
                "in_existing",
                "cus_123",
                None,
                16000,
                4800,
                "eur",
                "pending",
                "2026-03-01T00:00:00+00:00",
                "2026-03-31T23:59:59+00:00",
                "2026-03-01T12:00:00+00:00",
            ),
        )

        status = self.handler._affiliate_process_commission(
            self.conn,
            stripe=_FakeStripe([]),
            stripe_event_id="evt_over_cap",
            stripe_customer_id="cus_123",
            amount_paid_cents=1000,
            currency="eur",
            invoice_id="in_over_cap",
            period_start="2026-04-01T00:00:00+00:00",
            period_end="2026-04-30T23:59:59+00:00",
        )

        self.assertEqual(status, "skipped")
        expected_lock_key = self.handler._affiliate_commission_lock_key("affiliate_one")
        self.assertEqual(
            self.conn.lock_calls,
            [("lock", expected_lock_key), ("unlock", expected_lock_key)],
        )
        self.assertEqual(self.conn.transaction_events, ["begin", "commit"])

        row = self.raw_conn.execute(
            """
            SELECT commission_cents, status
            FROM affiliate_commissions
            WHERE stripe_event_id = ?
            """,
            ("evt_over_cap",),
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(row["commission_cents"], 300)
        self.assertEqual(row["status"], "skipped")

    def test_connected_affiliate_transfer_failure_and_legacy_failed_row_are_retried(self) -> None:
        self._insert_affiliate_fixture(stripe_account_id="acct_123")
        stripe = _FakeStripe(
            [
                RuntimeError("temporary transfer failure"),
                "tr_retry_old",
                "tr_retry_new",
            ]
        )

        first_status = self.handler._affiliate_process_commission(
            self.conn,
            stripe=stripe,
            stripe_event_id="evt_first",
            stripe_customer_id="cus_123",
            amount_paid_cents=1000,
            currency="eur",
            invoice_id="in_first",
            period_start="2026-04-01T00:00:00+00:00",
            period_end="2026-04-30T23:59:59+00:00",
        )

        self.assertEqual(first_status, "pending")
        first_row = self.raw_conn.execute(
            """
            SELECT status, stripe_transfer_id, error_message
            FROM affiliate_commissions
            WHERE stripe_event_id = ?
            """,
            ("evt_first",),
        ).fetchone()
        self.assertEqual(first_row["status"], "pending")
        self.assertIsNone(first_row["stripe_transfer_id"])
        self.assertIn("temporary transfer failure", first_row["error_message"])
        self.assertEqual(
            stripe.Transfer.calls[0]["idempotency_key"],
            "affiliate-transfer:1",
        )

        # Simulate a historical row that was marked failed by the previous implementation.
        self.raw_conn.execute(
            "UPDATE affiliate_commissions SET status = 'failed' WHERE stripe_event_id = ?",
            ("evt_first",),
        )

        second_status = self.handler._affiliate_process_commission(
            self.conn,
            stripe=stripe,
            stripe_event_id="evt_second",
            stripe_customer_id="cus_123",
            amount_paid_cents=2000,
            currency="eur",
            invoice_id="in_second",
            period_start="2026-05-01T00:00:00+00:00",
            period_end="2026-05-31T23:59:59+00:00",
        )

        self.assertEqual(second_status, "transferred")
        rows = self.raw_conn.execute(
            """
            SELECT stripe_event_id, status, stripe_transfer_id, error_message
            FROM affiliate_commissions
            ORDER BY id ASC
            """
        ).fetchall()
        self.assertEqual(
            [(row["stripe_event_id"], row["status"], row["stripe_transfer_id"]) for row in rows],
            [
                ("evt_first", "transferred", "tr_retry_old"),
                ("evt_second", "transferred", "tr_retry_new"),
            ],
        )
        self.assertEqual([row["error_message"] for row in rows], [None, None])
        self.assertEqual(
            [call["transfer_group"] for call in stripe.Transfer.calls],
            ["evt_first", "evt_first", "evt_second"],
        )
        self.assertEqual(
            [call["idempotency_key"] for call in stripe.Transfer.calls],
            [
                "affiliate-transfer:1",
                "affiliate-transfer:1",
                "affiliate-transfer:2",
            ],
        )


if __name__ == "__main__":
    unittest.main()
