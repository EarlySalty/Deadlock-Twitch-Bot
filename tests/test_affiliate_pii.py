from __future__ import annotations

import json
import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from bot.dashboard.affiliate.affiliate_pii import AffiliatePII
from bot.dashboard.affiliate_mixin import _DashboardAffiliateMixin


class _ConnContext:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def __enter__(self) -> sqlite3.Connection:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _CompatSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        return self._conn.execute(str(sql or "").replace("%s", "?"), tuple(params or ()))

    def executemany(self, sql: str, params=None):
        return self._conn.executemany(str(sql or "").replace("%s", "?"), params or ())

    def commit(self) -> None:
        self._conn.commit()

    def rollback(self) -> None:
        self._conn.rollback()

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _FakeCrypto:
    def __init__(self) -> None:
        self.encrypt_calls: list[tuple[str, str]] = []
        self.decrypt_calls: list[tuple[bytes, str]] = []

    def encrypt_field(self, plaintext: str, aad: str, kid: str = "v1") -> bytes:
        del kid
        self.encrypt_calls.append((plaintext, aad))
        return f"enc::{aad}::{plaintext}".encode("utf-8")

    def decrypt_field(self, blob: bytes, aad: str) -> str:
        self.decrypt_calls.append((bytes(blob), aad))
        prefix = f"enc::{aad}::".encode("utf-8")
        if not bytes(blob).startswith(prefix):
            raise AssertionError(f"Unexpected blob for AAD {aad}: {blob!r}")
        return bytes(blob)[len(prefix):].decode("utf-8")


class _AffiliateProfileRequest:
    def __init__(self, body: dict) -> None:
        self._body = dict(body)

    async def json(self) -> dict:
        return dict(self._body)


class _AffiliateProfileHarness(_DashboardAffiliateMixin):
    def _get_affiliate_session(self, _request):
        return {
            "twitch_login": "affiliate_one",
            "twitch_user_id": "1001",
            "display_name": "Affiliate One",
        }


class AffiliatePIITests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
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
            """
        )
        self.conn.execute(
            """
            INSERT INTO affiliate_accounts (
                twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                address_city, address_zip, address_country, stripe_connect_status,
                created_at, updated_at, is_active
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "affiliate_one",
                "1001",
                "Affiliate One",
                "",
                "",
                "",
                "",
                "",
                "",
                "pending",
                "2026-03-01T10:00:00+00:00",
                "2026-03-01T10:00:00+00:00",
                1,
            ),
        )
        self.conn.commit()

    def tearDown(self) -> None:
        self.conn.close()

    def test_save_and_load_roundtrip_encrypts_expected_fields(self) -> None:
        crypto = _FakeCrypto()
        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=crypto):
            AffiliatePII.save_pii(
                self.compat_conn,
                "affiliate_one",
                {
                    "full_name": "Affiliate One GmbH",
                    "email": "affiliate@example.com",
                    "address_line1": "Musterstr. 1",
                    "address_city": "Berlin",
                    "address_zip": "10115",
                    "address_country": "de",
                    "tax_id": "DE123456789",
                    "ust_status": "regelbesteuert",
                },
            )
            payload = AffiliatePII.load_pii(self.compat_conn, "affiliate_one")

        raw = self.conn.execute(
            "SELECT email_enc, full_name_enc, address_country, ust_status FROM affiliate_pii WHERE twitch_login = ?",
            ("affiliate_one",),
        ).fetchone()
        self.assertIsNotNone(raw["email_enc"])
        self.assertNotEqual(raw["email_enc"], b"affiliate@example.com")
        self.assertEqual(payload["full_name"], "Affiliate One GmbH")
        self.assertEqual(payload["email"], "affiliate@example.com")
        self.assertEqual(payload["address_line1"], "Musterstr. 1")
        self.assertEqual(payload["address_city"], "Berlin")
        self.assertEqual(payload["address_zip"], "10115")
        self.assertEqual(payload["address_country"], "DE")
        self.assertEqual(payload["tax_id"], "DE123456789")
        self.assertEqual(payload["ust_status"], "regelbesteuert")
        self.assertIn(
            ("affiliate@example.com", "affiliate_pii|email|affiliate_one"),
            crypto.encrypt_calls,
        )
        self.assertIn(
            (bytes(raw["email_enc"]), "affiliate_pii|email|affiliate_one"),
            crypto.decrypt_calls,
        )

    def test_migrate_from_plaintext_moves_values_and_clears_affiliate_accounts(self) -> None:
        self.conn.execute(
            """
            UPDATE affiliate_accounts
            SET email = ?, full_name = ?, address_line1 = ?, address_city = ?,
                address_zip = ?, address_country = ?
            WHERE twitch_login = ?
            """,
            (
                "legacy@example.com",
                "Legacy Partner",
                "Altbau 5",
                "Hamburg",
                "20095",
                "DE",
                "affiliate_one",
            ),
        )
        self.conn.commit()

        crypto = _FakeCrypto()
        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=crypto):
            migrated = AffiliatePII.migrate_from_plaintext(self.compat_conn)
            payload = AffiliatePII.load_pii(self.compat_conn, "affiliate_one")

        row = self.conn.execute(
            """
            SELECT email, full_name, address_line1, address_city, address_zip, address_country
            FROM affiliate_accounts
            WHERE twitch_login = ?
            """,
            ("affiliate_one",),
        ).fetchone()
        self.assertEqual(migrated, 1)
        self.assertEqual(payload["email"], "legacy@example.com")
        self.assertEqual(payload["full_name"], "Legacy Partner")
        self.assertEqual(payload["address_line1"], "Altbau 5")
        self.assertEqual(payload["address_city"], "Hamburg")
        self.assertEqual(payload["address_zip"], "20095")
        self.assertEqual(payload["address_country"], "DE")
        self.assertEqual(row["email"], "")
        self.assertEqual(row["full_name"], "")
        self.assertEqual(row["address_line1"], "")
        self.assertEqual(row["address_city"], "")
        self.assertEqual(row["address_zip"], "")
        self.assertEqual(row["address_country"], "")
        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=crypto):
            self.assertEqual(AffiliatePII.migrate_from_plaintext(self.compat_conn), 0)

    async def test_affiliate_api_me_reads_decrypted_profile_fields(self) -> None:
        crypto = _FakeCrypto()
        handler = _AffiliateProfileHarness()
        handler._affiliate_ensure_tables = lambda _conn: None  # type: ignore[method-assign]
        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=crypto):
            AffiliatePII.save_pii(
                self.compat_conn,
                "affiliate_one",
                {
                    "full_name": "Affiliate One",
                    "email": "partner@example.com",
                    "address_line1": "Main Street 1",
                    "address_city": "Berlin",
                    "address_zip": "10115",
                    "address_country": "DE",
                    "tax_id": "DE123",
                    "ust_status": "kleinunternehmer",
                },
            )
            self.conn.commit()
            with patch(
                "bot.dashboard.affiliate_mixin.storage.readonly_connection",
                return_value=_ConnContext(self.compat_conn),
            ):
                response = await handler._affiliate_api_me(SimpleNamespace())

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["email"], "partner@example.com")
        self.assertEqual(payload["full_name"], "Affiliate One")
        self.assertEqual(payload["address_line1"], "Main Street 1")
        self.assertEqual(payload["address_city"], "Berlin")
        self.assertEqual(payload["address_zip"], "10115")
        self.assertEqual(payload["address_country"], "DE")
        self.assertEqual(payload["tax_id"], "DE123")
        self.assertEqual(payload["ust_status"], "kleinunternehmer")

    async def test_affiliate_profile_put_writes_only_encrypted_pii(self) -> None:
        crypto = _FakeCrypto()
        handler = _AffiliateProfileHarness()
        handler._affiliate_ensure_tables = lambda _conn: None  # type: ignore[method-assign]
        request = _AffiliateProfileRequest(
            {
                "full_name": "Updated Affiliate",
                "email": "updated@example.com",
                "address_line1": "Neue Str. 8",
                "address_city": "Munich",
                "address_zip": "80331",
                "address_country": "de",
                "tax_id": "DE999",
                "ust_status": "regelbesteuert",
            }
        )

        with patch("bot.dashboard.affiliate.affiliate_pii.get_crypto", return_value=crypto):
            with patch(
                "bot.dashboard.affiliate_mixin.storage.transaction",
                return_value=_ConnContext(self.compat_conn),
            ):
                response = await handler._affiliate_api_profile_update(request)

        payload = json.loads(response.body.decode("utf-8"))
        raw_account = self.conn.execute(
            "SELECT email, full_name, address_line1, address_city, address_zip, address_country FROM affiliate_accounts WHERE twitch_login = ?",
            ("affiliate_one",),
        ).fetchone()
        raw_pii = self.conn.execute(
            "SELECT email_enc, full_name_enc, tax_id_enc, address_country, ust_status FROM affiliate_pii WHERE twitch_login = ?",
            ("affiliate_one",),
        ).fetchone()
        self.assertEqual(response.status, 200)
        self.assertTrue(payload["ok"])
        self.assertEqual(payload["profile"]["email"], "updated@example.com")
        self.assertEqual(payload["profile"]["ust_status"], "regelbesteuert")
        self.assertEqual(raw_account["email"], "")
        self.assertEqual(raw_account["full_name"], "")
        self.assertEqual(raw_account["address_line1"], "")
        self.assertEqual(raw_account["address_city"], "")
        self.assertEqual(raw_account["address_zip"], "")
        self.assertEqual(raw_account["address_country"], "")
        self.assertIsNotNone(raw_pii["email_enc"])
        self.assertIsNotNone(raw_pii["full_name_enc"])
        self.assertIsNotNone(raw_pii["tax_id_enc"])
        self.assertEqual(raw_pii["address_country"], "DE")
        self.assertEqual(raw_pii["ust_status"], "regelbesteuert")

    async def test_affiliate_profile_put_rejects_invalid_ust_status(self) -> None:
        handler = _AffiliateProfileHarness()
        handler._affiliate_ensure_tables = lambda _conn: None  # type: ignore[method-assign]
        request = _AffiliateProfileRequest({"ust_status": "foo"})

        response = await handler._affiliate_api_profile_update(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 400)
        self.assertEqual(payload["error"], "invalid_ust_status")


if __name__ == "__main__":
    unittest.main()
