from __future__ import annotations

import json
import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from bot.dashboard.affiliate.affiliate_pii import AffiliatePII
from bot.dashboard.affiliate.gutschrift import AffiliateGutschriftService
from bot.dashboard.affiliate_mixin import _DashboardAffiliateMixin


class _ConnContext:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def __enter__(self) -> sqlite3.Connection:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _ListCursor:
    def __init__(self, rows):
        self._rows = list(rows)

    def fetchone(self):
        return self._rows[0] if self._rows else None

    def fetchall(self):
        return list(self._rows)


class _CompatSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "")
        sql_params = tuple(params or ())
        if "FROM information_schema.columns" in sql_text:
            table_name = str(sql_params[0] if sql_params else "").strip()
            pragma_rows = self._conn.execute(f"PRAGMA table_info({table_name})").fetchall()
            return _ListCursor(
                [((row["name"] if hasattr(row, "keys") else row[1]),) for row in pragma_rows]
            )
        return self._conn.execute(sql_text.replace("%s", "?"), sql_params)

    def executemany(self, sql: str, params=None):
        return self._conn.executemany(str(sql or "").replace("%s", "?"), params or ())

    def commit(self) -> None:
        self._conn.commit()

    def rollback(self) -> None:
        self._conn.rollback()

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _FakeCrypto:
    def encrypt_field(self, plaintext: str, aad: str, kid: str = "v1") -> bytes:
        del kid
        return f"enc::{aad}::{plaintext}".encode("utf-8")

    def decrypt_field(self, blob: bytes, aad: str) -> str:
        prefix = f"enc::{aad}::".encode("utf-8")
        if not bytes(blob).startswith(prefix):
            raise AssertionError(f"Unexpected blob for {aad}: {blob!r}")
        return bytes(blob)[len(prefix):].decode("utf-8")


class _RecordingEmailSender:
    def __init__(self) -> None:
        self.calls: list[dict[str, object]] = []

    def send_gutschrift(self, **kwargs) -> None:
        self.calls.append(dict(kwargs))


class _JsonRequest:
    def __init__(self, body: dict[str, object]) -> None:
        self._body = dict(body)
        self.match_info: dict[str, str] = {}

    async def json(self) -> dict[str, object]:
        return dict(self._body)


class _AffiliateHarness(_DashboardAffiliateMixin):
    def _get_affiliate_session(self, _request):
        return {
            "twitch_login": "affiliate_one",
            "twitch_user_id": "1001",
            "display_name": "Affiliate One",
        }

    def _require_v2_admin_api(self, _request):
        return None

    def _load_secret_value(self, *keys: str) -> str:
        del keys
        return ""


class AffiliateGutschriftTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:", check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        self.compat_conn = _CompatSqliteConn(self.conn)
        self.conn.executescript(
            """
            CREATE TABLE affiliate_accounts (
                twitch_login TEXT PRIMARY KEY,
                twitch_user_id TEXT NOT NULL,
                display_name TEXT,
                email TEXT NOT NULL,
                full_name TEXT NOT NULL,
                address_line1 TEXT NOT NULL,
                address_city TEXT NOT NULL,
                address_zip TEXT NOT NULL,
                address_country TEXT NOT NULL DEFAULT '',
                stripe_account_id TEXT,
                stripe_connected_at TEXT,
                stripe_connect_status TEXT DEFAULT 'pending',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE affiliate_pii (
                twitch_login TEXT PRIMARY KEY REFERENCES affiliate_accounts(twitch_login),
                full_name_enc BLOB,
                email_enc BLOB,
                address_line1_enc BLOB,
                address_city_enc BLOB,
                address_zip_enc BLOB,
                tax_id_enc BLOB,
                address_country TEXT NOT NULL DEFAULT 'DE',
                ust_status TEXT NOT NULL DEFAULT 'unknown',
                updated_at TEXT NOT NULL
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
                currency TEXT NOT NULL DEFAULT 'eur',
                status TEXT NOT NULL DEFAULT 'pending',
                period_start TEXT,
                period_end TEXT,
                created_at TEXT NOT NULL,
                transferred_at TEXT,
                error_message TEXT
            );
            CREATE TABLE affiliate_gutschrift_counter (
                year_month TEXT PRIMARY KEY,
                last_seq INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE affiliate_gutschriften (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gutschrift_number TEXT NOT NULL UNIQUE,
                affiliate_twitch_login TEXT NOT NULL,
                period_year INTEGER NOT NULL,
                period_month INTEGER NOT NULL,
                net_amount_cents INTEGER NOT NULL,
                vat_rate_percent NUMERIC(5,2) NOT NULL DEFAULT 0,
                vat_amount_cents INTEGER NOT NULL DEFAULT 0,
                gross_amount_cents INTEGER NOT NULL,
                affiliate_name TEXT NOT NULL,
                affiliate_address TEXT NOT NULL,
                affiliate_tax_id TEXT,
                affiliate_ust_status TEXT NOT NULL,
                issuer_name TEXT NOT NULL,
                issuer_address TEXT NOT NULL,
                issuer_tax_id TEXT NOT NULL,
                pdf_blob BLOB,
                pdf_generated_at TEXT,
                email_sent_at TEXT,
                email_error TEXT,
                commission_ids TEXT,
                created_at TEXT NOT NULL,
                UNIQUE (affiliate_twitch_login, period_year, period_month)
            );
            """
        )
        self._insert_affiliate("affiliate_one", "1001", "Affiliate One")

    def tearDown(self) -> None:
        self.conn.close()

    def _insert_affiliate(self, login: str, user_id: str, display_name: str) -> None:
        self.conn.execute(
            """
            INSERT INTO affiliate_accounts (
                twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                address_city, address_zip, address_country, stripe_connect_status,
                created_at, updated_at, is_active
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                login,
                user_id,
                display_name,
                "",
                "",
                "",
                "",
                "",
                "",
                "pending",
                "2026-01-01T10:00:00+00:00",
                "2026-01-01T10:00:00+00:00",
                1,
            ),
        )
        self.conn.commit()

    def _insert_commission(
        self,
        *,
        affiliate_login: str = "affiliate_one",
        stripe_event_id: str,
        commission_cents: int,
        created_at: str,
        status: str = "transferred",
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO affiliate_commissions (
                affiliate_twitch_login, streamer_login, stripe_event_id, stripe_invoice_id,
                stripe_customer_id, stripe_transfer_id, brutto_cents, commission_cents,
                currency, status, period_start, period_end, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                affiliate_login,
                "streamer_one",
                stripe_event_id,
                f"in_{stripe_event_id}",
                "cus_123",
                None,
                commission_cents * 3,
                commission_cents,
                "eur",
                status,
                "2026-02-01T00:00:00+00:00",
                "2026-02-28T23:59:59+00:00",
                created_at,
            ),
        )
        self.conn.commit()

    def _save_profile(self, affiliate_login: str, data: dict[str, object]) -> None:
        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=_FakeCrypto()):
            AffiliatePII.save_pii(self.compat_conn, affiliate_login, data)
            self.conn.commit()

    @staticmethod
    def _seller() -> dict[str, str]:
        return {
            "name": "Deadlock Partner Network",
            "company": "EarlySalty GmbH",
            "street": "Issuer Street 9",
            "postal_code": "40213",
            "city": "Duesseldorf",
            "country": "DE",
            "tax_id": "DE999999999",
        }

    def test_generate_for_period_blocks_when_ust_status_is_unknown(self) -> None:
        self._insert_commission(
            stripe_event_id="evt_blocked",
            commission_cents=1500,
            created_at="2026-02-10T12:00:00+00:00",
        )
        self._save_profile(
            "affiliate_one",
            {
                "full_name": "Affiliate One",
                "email": "affiliate@example.com",
                "address_line1": "Musterstr. 1",
                "address_city": "Berlin",
                "address_zip": "10115",
                "address_country": "DE",
                "tax_id": "12/345/67890",
                "ust_status": "unknown",
            },
        )

        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=_FakeCrypto()):
            result = AffiliateGutschriftService.generate_for_period(
                self.compat_conn,
                affiliate_login="affiliate_one",
                year=2026,
                month=2,
                seller=self._seller(),
            )

        self.assertFalse(result["ok"])
        self.assertEqual(result["status"], "blocked")
        row = self.conn.execute("SELECT COUNT(*) AS cnt FROM affiliate_gutschriften").fetchone()
        self.assertEqual(row["cnt"], 0)

    def test_generate_for_period_creates_snapshot_with_monthly_number(self) -> None:
        self._insert_commission(
            stripe_event_id="evt_small_1",
            commission_cents=1200,
            created_at="2026-02-10T12:00:00+00:00",
        )
        self._insert_commission(
            stripe_event_id="evt_small_2",
            commission_cents=800,
            created_at="2026-02-15T08:00:00+00:00",
        )
        self._save_profile(
            "affiliate_one",
            {
                "full_name": "Affiliate One",
                "email": "affiliate@example.com",
                "address_line1": "Musterstr. 1",
                "address_city": "Berlin",
                "address_zip": "10115",
                "address_country": "DE",
                "tax_id": "12/345/67890",
                "ust_status": "kleinunternehmer",
            },
        )

        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=_FakeCrypto()):
            with patch.object(
                AffiliateGutschriftService,
                "generate_gutschrift_pdf",
                return_value=b"%PDF-small",
            ):
                result = AffiliateGutschriftService.generate_for_period(
                    self.compat_conn,
                    affiliate_login="affiliate_one",
                    year=2026,
                    month=2,
                    seller=self._seller(),
                )

        self.assertTrue(result["ok"])
        document = result["document"]
        self.assertEqual(document["status"], "generated")
        self.assertEqual(document["net_amount_cents"], 2000)
        self.assertEqual(document["vat_amount_cents"], 0)
        self.assertEqual(document["gross_amount_cents"], 2000)
        self.assertEqual(document["gutschrift_number"], "GS-202602-0001")
        self.assertEqual(document["commission_count"], 2)
        self.assertEqual(
            document["note_text"],
            "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet.",
        )

        row = self.conn.execute(
            """
            SELECT *
            FROM affiliate_gutschriften
            WHERE affiliate_twitch_login = ? AND period_year = ? AND period_month = ?
            """,
            ("affiliate_one", 2026, 2),
        ).fetchone()
        self.assertEqual(row["affiliate_name"], "Affiliate One")
        self.assertEqual(row["affiliate_address"], "Musterstr. 1\n10115 Berlin\nDE")
        self.assertEqual(row["affiliate_tax_id"], "Steuernummer: 12/345/67890")
        self.assertEqual(row["affiliate_ust_status"], "kleinunternehmer")
        self.assertEqual(row["issuer_name"], "EarlySalty GmbH")
        self.assertEqual(row["issuer_address"], "Issuer Street 9\n40213 Duesseldorf\nDE")
        self.assertEqual(row["issuer_tax_id"], "DE999999999")
        self.assertEqual(row["commission_ids"], "[1,2]")
        self.assertIsNotNone(row["pdf_generated_at"])
        self.assertIsNone(row["email_sent_at"])
        self.assertIsNone(row["email_error"])

    def test_generate_monthly_gutschriften_numbers_are_sequential_per_month(self) -> None:
        self._insert_affiliate("affiliate_two", "1002", "Affiliate Two")
        self._insert_commission(
            stripe_event_id="evt_seq_1",
            commission_cents=1000,
            created_at="2026-02-05T12:00:00+00:00",
        )
        self._insert_commission(
            affiliate_login="affiliate_two",
            stripe_event_id="evt_seq_2",
            commission_cents=900,
            created_at="2026-02-18T12:00:00+00:00",
        )
        self._save_profile(
            "affiliate_one",
            {
                "full_name": "Affiliate One",
                "email": "affiliate@example.com",
                "address_line1": "Musterstr. 1",
                "address_city": "Berlin",
                "address_zip": "10115",
                "address_country": "DE",
                "tax_id": "12/345/67890",
                "ust_status": "kleinunternehmer",
            },
        )
        self._save_profile(
            "affiliate_two",
            {
                "full_name": "Affiliate Two",
                "email": "affiliate2@example.com",
                "address_line1": "Musterweg 2",
                "address_city": "Koeln",
                "address_zip": "50667",
                "address_country": "DE",
                "tax_id": "98/765/43210",
                "ust_status": "kleinunternehmer",
            },
        )

        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=_FakeCrypto()):
            with patch.object(
                AffiliateGutschriftService,
                "generate_gutschrift_pdf",
                return_value=b"%PDF-seq",
            ):
                results = AffiliateGutschriftService.generate_monthly_gutschriften(
                    self.compat_conn,
                    2026,
                    2,
                    seller=self._seller(),
                )

        numbers = sorted(result["document"]["gutschrift_number"] for result in results if result["ok"])
        self.assertEqual(numbers, ["GS-202602-0001", "GS-202602-0002"])

    def test_generate_for_period_applies_19_percent_vat_and_sends_email(self) -> None:
        self._insert_commission(
            stripe_event_id="evt_regular",
            commission_cents=1000,
            created_at="2026-02-20T10:00:00+00:00",
        )
        self._save_profile(
            "affiliate_one",
            {
                "full_name": "Affiliate One",
                "email": "affiliate@example.com",
                "address_line1": "Musterstr. 1",
                "address_city": "Berlin",
                "address_zip": "10115",
                "address_country": "DE",
                "tax_id": "12/345/67890",
                "vat_id": "DE123456789",
                "ust_status": "regelbesteuert",
            },
        )
        sender = _RecordingEmailSender()

        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=_FakeCrypto()):
            with patch.object(
                AffiliateGutschriftService,
                "generate_gutschrift_pdf",
                return_value=b"%PDF-regular",
            ):
                result = AffiliateGutschriftService.generate_for_period(
                    self.compat_conn,
                    affiliate_login="affiliate_one",
                    year=2026,
                    month=2,
                    email_sender=sender,
                    seller=self._seller(),
                )

        self.assertTrue(result["ok"])
        document = result["document"]
        self.assertEqual(document["status"], "emailed")
        self.assertEqual(document["net_amount_cents"], 1000)
        self.assertEqual(document["vat_amount_cents"], 190)
        self.assertEqual(document["gross_amount_cents"], 1190)
        self.assertEqual(document["gutschrift_number"], "GS-202602-0001")
        self.assertEqual(len(sender.calls), 1)
        row = self.conn.execute(
            "SELECT email_sent_at, email_error FROM affiliate_gutschriften WHERE id = ?",
            (document["id"],),
        ).fetchone()
        self.assertIsNotNone(row["email_sent_at"])
        self.assertIsNone(row["email_error"])

    def test_ensure_schema_supports_legacy_counter_and_adds_snapshot_columns(self) -> None:
        legacy_conn = sqlite3.connect(":memory:", check_same_thread=False)
        legacy_conn.row_factory = sqlite3.Row
        legacy_conn.executescript(
            """
            CREATE TABLE affiliate_gutschrift_counter (
                counter_year INTEGER PRIMARY KEY,
                last_counter INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE affiliate_gutschriften (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                affiliate_twitch_login TEXT NOT NULL,
                period_year INTEGER NOT NULL,
                period_month INTEGER NOT NULL,
                period_start TEXT NOT NULL,
                period_end TEXT NOT NULL,
                gutschrift_number TEXT UNIQUE,
                currency TEXT NOT NULL DEFAULT 'eur',
                status TEXT NOT NULL DEFAULT 'generated',
                ust_status TEXT NOT NULL DEFAULT 'unknown',
                commission_count INTEGER NOT NULL DEFAULT 0,
                net_amount_cents INTEGER NOT NULL DEFAULT 0,
                vat_rate_basis_points INTEGER NOT NULL DEFAULT 0,
                vat_amount_cents INTEGER NOT NULL DEFAULT 0,
                gross_amount_cents INTEGER NOT NULL DEFAULT 0,
                note_text TEXT,
                pdf_blob BLOB,
                last_error TEXT,
                generated_at TEXT,
                emailed_at TEXT,
                updated_at TEXT NOT NULL,
                UNIQUE (affiliate_twitch_login, period_year, period_month)
            );
            """
        )
        legacy_compat = _CompatSqliteConn(legacy_conn)
        AffiliateGutschriftService.ensure_schema(legacy_compat)
        legacy_conn.execute(
            """
            INSERT INTO affiliate_gutschrift_counter (counter_year, last_counter, updated_at)
            VALUES (?, ?, ?)
            """,
            (202602, 3, "2026-03-01T00:00:00+00:00"),
        )
        next_number = AffiliateGutschriftService._next_gutschrift_number(legacy_compat, "202602")
        columns = {
            row["name"]
            for row in legacy_conn.execute("PRAGMA table_info(affiliate_gutschriften)").fetchall()
        }
        legacy_conn.close()

        self.assertEqual(next_number, "GS-202602-0004")
        self.assertIn("affiliate_name", columns)
        self.assertIn("issuer_tax_id", columns)
        self.assertIn("commission_ids", columns)

    async def test_affiliate_api_gutschriften_returns_documents_and_readiness(self) -> None:
        self._insert_commission(
            stripe_event_id="evt_api",
            commission_cents=900,
            created_at="2026-02-12T09:00:00+00:00",
        )
        self._save_profile(
            "affiliate_one",
            {
                "full_name": "Affiliate One",
                "email": "affiliate@example.com",
                "address_line1": "Musterstr. 1",
                "address_city": "Berlin",
                "address_zip": "10115",
                "address_country": "DE",
                "tax_id": "12/345/67890",
                "ust_status": "kleinunternehmer",
            },
        )
        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=_FakeCrypto()):
            with patch.object(
                AffiliateGutschriftService,
                "generate_gutschrift_pdf",
                return_value=b"%PDF-api",
            ):
                AffiliateGutschriftService.generate_for_period(
                    self.compat_conn,
                    affiliate_login="affiliate_one",
                    year=2026,
                    month=2,
                    seller=self._seller(),
                )
                self.conn.commit()

            handler = _AffiliateHarness()
            handler._affiliate_ensure_tables = lambda _conn: None  # type: ignore[method-assign]
            with patch(
                "bot.dashboard.affiliate_mixin.storage.readonly_connection",
                return_value=_ConnContext(self.compat_conn),
            ):
                response = await handler._affiliate_api_gutschriften(SimpleNamespace())

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertTrue(payload["readiness"]["can_generate"])
        self.assertEqual(len(payload["gutschriften"]), 1)
        self.assertEqual(payload["gutschriften"][0]["status"], "generated")
        self.assertEqual(payload["gutschriften"][0]["gutschrift_number"], "GS-202602-0001")
        self.assertEqual(payload["gutschriften"][0]["download_path"], "/twitch/api/affiliate/gutschriften/1/pdf")

    async def test_affiliate_api_gutschrift_pdf_returns_stored_blob(self) -> None:
        self.conn.execute(
            """
            INSERT INTO affiliate_gutschriften (
                gutschrift_number, affiliate_twitch_login, period_year, period_month,
                net_amount_cents, vat_rate_percent, vat_amount_cents, gross_amount_cents,
                affiliate_name, affiliate_address, affiliate_tax_id, affiliate_ust_status,
                issuer_name, issuer_address, issuer_tax_id, pdf_blob, pdf_generated_at,
                email_sent_at, email_error, commission_ids, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "GS-202602-0007",
                "affiliate_one",
                2026,
                2,
                900,
                "0.00",
                0,
                900,
                "Affiliate One",
                "Musterstr. 1\n10115 Berlin\nDE",
                "Steuernummer: 12/345/67890",
                "kleinunternehmer",
                "EarlySalty GmbH",
                "Issuer Street 9\n40213 Duesseldorf\nDE",
                "DE999999999",
                b"%PDF-download",
                "2026-03-01T10:00:00+00:00",
                None,
                None,
                "[42]",
                "2026-03-01T10:00:00+00:00",
            ),
        )
        self.conn.commit()

        handler = _AffiliateHarness()
        handler._affiliate_ensure_tables = lambda _conn: None  # type: ignore[method-assign]
        request = SimpleNamespace(match_info={"gutschrift_id": "1"})
        with patch(
            "bot.dashboard.affiliate_mixin.storage.readonly_connection",
            return_value=_ConnContext(self.compat_conn),
        ):
            response = await handler._affiliate_api_gutschrift_pdf(request)

        self.assertEqual(response.status, 200)
        self.assertEqual(response.content_type, "application/pdf")
        self.assertEqual(response.body, b"%PDF-download")

    async def test_affiliate_admin_trigger_generates_for_month_without_login_filter(self) -> None:
        self._insert_commission(
            stripe_event_id="evt_admin",
            commission_cents=1100,
            created_at="2026-02-08T10:00:00+00:00",
        )
        self._save_profile(
            "affiliate_one",
            {
                "full_name": "Affiliate One",
                "email": "affiliate@example.com",
                "address_line1": "Musterstr. 1",
                "address_city": "Berlin",
                "address_zip": "10115",
                "address_country": "DE",
                "tax_id": "12/345/67890",
                "ust_status": "kleinunternehmer",
            },
        )
        handler = _AffiliateHarness()
        handler._affiliate_ensure_tables = lambda _conn: None  # type: ignore[method-assign]
        request = _JsonRequest({"year": 2026, "month": 2})

        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=_FakeCrypto()):
            with patch.object(
                AffiliateGutschriftService,
                "generate_gutschrift_pdf",
                return_value=b"%PDF-admin",
            ):
                with patch(
                    "bot.dashboard.affiliate_mixin.storage.transaction",
                    return_value=_ConnContext(self.compat_conn),
                ):
                    response = await handler._affiliate_api_gutschrift_trigger(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertTrue(payload["ok"])
        self.assertEqual(len(payload["results"]), 1)
        self.assertEqual(
            payload["results"][0]["document"]["gutschrift_number"],
            "GS-202602-0001",
        )


if __name__ == "__main__":
    unittest.main()
