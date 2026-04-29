from __future__ import annotations

import sqlite3
import unittest

from bot.storage.partner_registry import (
    archive_active_partner,
    bulk_update_partner_flags,
    migrate_legacy_partner_registry,
    reactivate_partner,
    set_streamer_archive_state,
    upsert_streamer_identity,
)


class _SqlitePgCompatConnection:
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


def _make_conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE twitch_streamer_identities (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL,
            discord_user_id TEXT,
            discord_display_name TEXT,
            is_on_discord INTEGER DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE UNIQUE INDEX idx_twitch_streamer_identities_discord_user
        ON twitch_streamer_identities(discord_user_id)
        WHERE discord_user_id IS NOT NULL AND discord_user_id <> ''
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_partners (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL,
            require_discord_link INTEGER DEFAULT 0,
            last_description TEXT,
            last_link_ok INTEGER,
            added_by TEXT,
            last_link_checked_at TEXT,
            next_link_check_at TEXT,
            manual_verified_permanent INTEGER DEFAULT 0,
            manual_verified_until TEXT,
            manual_verified_at TEXT,
            manual_partner_opt_out INTEGER DEFAULT 0,
            raid_bot_enabled INTEGER DEFAULT 0,
            silent_ban INTEGER DEFAULT 0,
            silent_raid INTEGER DEFAULT 0,
            live_ping_role_id TEXT,
            live_ping_enabled INTEGER DEFAULT 1,
            partnered_at TEXT,
            departnered_at TEXT,
            technical_pause_reason TEXT,
            status TEXT NOT NULL DEFAULT 'active'
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_streamers (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            require_discord_link INTEGER DEFAULT 0,
            next_link_check_at TEXT,
            discord_user_id TEXT,
            discord_display_name TEXT,
            is_on_discord INTEGER DEFAULT 0,
            manual_verified_permanent INTEGER DEFAULT 0,
            manual_verified_until TEXT,
            manual_verified_at TEXT,
            manual_partner_opt_out INTEGER DEFAULT 0,
            archived_at TEXT,
            raid_bot_enabled INTEGER DEFAULT 0,
            silent_ban INTEGER DEFAULT 0,
            silent_raid INTEGER DEFAULT 0,
            is_monitored_only INTEGER DEFAULT 0,
            live_ping_role_id TEXT,
            live_ping_enabled INTEGER DEFAULT 1,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            raid_enabled INTEGER DEFAULT 1,
            needs_reauth INTEGER DEFAULT 0
        )
        """
    )
    return conn


class PartnerRegistrySemanticsTests(unittest.TestCase):
    def test_migrate_legacy_partner_registry_prefers_active_partner_for_duplicate_discord_user(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.executemany(
            """
            INSERT INTO twitch_streamers (
                twitch_login,
                twitch_user_id,
                discord_user_id,
                discord_display_name,
                is_on_discord,
                manual_verified_at,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            [
                (
                    "scoutalpha",
                    "1001",
                    "662995601738170389",
                    "Shared User",
                    1,
                    None,
                    "2026-03-01T10:00:00+00:00",
                ),
                (
                    "partnerbravo",
                    "2002",
                    "662995601738170389",
                    "Shared User",
                    1,
                    "2026-03-05T10:00:00+00:00",
                    "2026-03-05T10:00:00+00:00",
                ),
            ],
        )

        stats = migrate_legacy_partner_registry(compat_conn)

        self.assertEqual(stats["identity_upserts"], 2)
        identity_rows = conn.execute(
            """
            SELECT twitch_user_id, twitch_login, discord_user_id
            FROM twitch_streamer_identities
            ORDER BY twitch_user_id
            """
        ).fetchall()
        self.assertEqual(identity_rows[0]["twitch_user_id"], "1001")
        self.assertIsNone(identity_rows[0]["discord_user_id"])
        self.assertEqual(identity_rows[1]["twitch_user_id"], "2002")
        self.assertEqual(identity_rows[1]["discord_user_id"], "662995601738170389")
        partner_row = conn.execute(
            "SELECT twitch_user_id, status FROM twitch_partners WHERE twitch_user_id = ?",
            ("2002",),
        ).fetchone()
        self.assertEqual(partner_row["status"], "active")

    def test_upsert_streamer_identity_reassigns_duplicate_discord_user(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.execute(
            """
            INSERT INTO twitch_streamer_identities (
                twitch_user_id, twitch_login, discord_user_id, discord_display_name, is_on_discord
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("1001", "alpha", "662995601738170389", "Alpha", 1),
        )

        upsert_streamer_identity(
            compat_conn,
            twitch_user_id="2002",
            twitch_login="bravo",
            discord_user_id="662995601738170389",
            discord_display_name="Bravo",
            is_on_discord=1,
        )

        rows = conn.execute(
            """
            SELECT twitch_user_id, discord_user_id, discord_display_name, is_on_discord
            FROM twitch_streamer_identities
            ORDER BY twitch_user_id
            """
        ).fetchall()
        self.assertEqual(rows[0]["twitch_user_id"], "1001")
        self.assertIsNone(rows[0]["discord_user_id"])
        self.assertIsNone(rows[0]["discord_display_name"])
        self.assertEqual(int(rows[0]["is_on_discord"]), 0)
        self.assertEqual(rows[1]["twitch_user_id"], "2002")
        self.assertEqual(rows[1]["discord_user_id"], "662995601738170389")
        self.assertEqual(rows[1]["discord_display_name"], "Bravo")
        self.assertEqual(int(rows[1]["is_on_discord"]), 1)

    def test_archive_active_partner_keeps_non_partner_table_empty(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.execute(
            """
            INSERT INTO twitch_streamer_identities (
                twitch_user_id, twitch_login, discord_user_id, discord_display_name, is_on_discord
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("1001", "alpha", "123", "Alpha", 1),
        )
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id, twitch_login, raid_bot_enabled, partnered_at, status
            ) VALUES (?, ?, 1, '2026-03-01T10:00:00+00:00', 'active')
            """,
            ("1001", "alpha"),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 1, 0)
            """,
            ("1001", "alpha"),
        )

        result = archive_active_partner(compat_conn, twitch_login="alpha")

        self.assertIsNotNone(result)
        status_row = conn.execute(
            "SELECT status, departnered_at FROM twitch_partners WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertEqual(status_row["status"], "archived")
        self.assertTrue(status_row["departnered_at"])
        streamer_count = conn.execute("SELECT COUNT(*) AS total FROM twitch_streamers").fetchone()
        self.assertEqual(int(streamer_count["total"]), 0)
        auth_row = conn.execute(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertEqual(int(auth_row["raid_enabled"]), 0)

    def test_archive_active_partner_keeps_historical_rows_unchanged(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id, twitch_login, partnered_at, departnered_at, status
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "alpha",
                "2026-02-01T10:00:00+00:00",
                "2026-02-10T10:00:00+00:00",
                "departnered",
            ),
        )
        history_id = int(conn.execute("SELECT last_insert_rowid()").fetchone()[0])
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id, twitch_login, raid_bot_enabled, partnered_at, status
            ) VALUES (?, ?, 1, '2026-03-01T10:00:00+00:00', 'active')
            """,
            ("1001", "alpha"),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 1, 0)
            """,
            ("1001", "alpha"),
        )

        result = archive_active_partner(compat_conn, twitch_login="alpha")

        self.assertIsNotNone(result)
        rows = conn.execute(
            """
            SELECT id, status
            FROM twitch_partners
            WHERE twitch_user_id = ?
            ORDER BY id
            """,
            ("1001",),
        ).fetchall()
        self.assertEqual(rows[0]["id"], history_id)
        self.assertEqual(rows[0]["status"], "departnered")
        self.assertEqual(rows[1]["status"], "archived")

    def test_set_streamer_archive_state_legacy_schema_reports_no_archive_value(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id, twitch_login, partnered_at, status
            ) VALUES (?, ?, ?, 'active')
            """,
            ("1001", "alpha", "2026-03-01T10:00:00+00:00"),
        )

        result = set_streamer_archive_state(
            compat_conn,
            twitch_login="alpha",
            archived=True,
        )

        self.assertIsNotNone(result)
        self.assertIsNone(result["archived_at"])

    def test_set_streamer_archive_state_unarchive_reenables_raid_auth(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.execute("ALTER TABLE twitch_partners ADD COLUMN admin_archived_at TEXT")
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id, twitch_login, partnered_at, admin_archived_at, status
            ) VALUES (?, ?, ?, ?, 'active')
            """,
            (
                "1001",
                "alpha",
                "2026-03-01T10:00:00+00:00",
                "2026-03-20T10:00:00+00:00",
            ),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 0, 0)
            """,
            ("1001", "alpha"),
        )

        result = set_streamer_archive_state(
            compat_conn,
            twitch_login="alpha",
            archived=False,
        )

        self.assertIsNotNone(result)
        partner_row = conn.execute(
            "SELECT admin_archived_at FROM twitch_partners WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertIsNone(partner_row["admin_archived_at"])
        auth_row = conn.execute(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertEqual(int(auth_row["raid_enabled"]), 1)

    def test_reactivate_partner_reenables_raid_auth_for_legacy_archived_rows(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id, twitch_login, raid_bot_enabled, partnered_at, departnered_at, status
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "alpha",
                1,
                "2026-03-01T10:00:00+00:00",
                "2026-03-20T10:00:00+00:00",
                "archived",
            ),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 0, 0)
            """,
            ("1001", "alpha"),
        )

        result = reactivate_partner(compat_conn, twitch_login="alpha")

        self.assertIsNotNone(result)
        active_row = conn.execute(
            """
            SELECT twitch_user_id, status
            FROM twitch_partners
            WHERE twitch_user_id = ? AND status = 'active'
            ORDER BY id DESC
            LIMIT 1
            """,
            ("1001",),
        ).fetchone()
        self.assertIsNotNone(active_row)
        auth_row = conn.execute(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertEqual(int(auth_row["raid_enabled"]), 1)

    def test_reactivate_partner_keeps_raid_auth_disabled_for_departnered_rows(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id, twitch_login, raid_bot_enabled, partnered_at, departnered_at, status
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "alpha",
                1,
                "2026-03-01T10:00:00+00:00",
                "2026-03-20T10:00:00+00:00",
                "departnered",
            ),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 0, 0)
            """,
            ("1001", "alpha"),
        )

        result = reactivate_partner(compat_conn, twitch_login="alpha")

        self.assertIsNotNone(result)
        auth_row = conn.execute(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertEqual(int(auth_row["raid_enabled"]), 0)

    def test_bulk_update_scope_all_only_mutates_active_rows(self) -> None:
        conn = _make_conn()
        compat_conn = _SqlitePgCompatConnection(conn)
        conn.executemany(
            """
            INSERT INTO twitch_partners (
                twitch_user_id,
                twitch_login,
                silent_raid,
                partnered_at,
                departnered_at,
                status
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            [
                ("1001", "alpha", 0, "2026-03-01T10:00:00+00:00", None, "active"),
                (
                    "2002",
                    "bravo",
                    0,
                    "2026-03-01T10:00:00+00:00",
                    "2026-03-10T10:00:00+00:00",
                    "archived",
                ),
            ],
        )

        total = bulk_update_partner_flags(compat_conn, scope="all", silent_raid=1)

        self.assertEqual(total, 1)
        rows = conn.execute(
            "SELECT twitch_user_id, silent_raid, status FROM twitch_partners ORDER BY twitch_user_id"
        ).fetchall()
        self.assertEqual(int(rows[0]["silent_raid"]), 1)
        self.assertEqual(rows[0]["status"], "active")
        self.assertEqual(int(rows[1]["silent_raid"]), 0)
        self.assertEqual(rows[1]["status"], "archived")


if __name__ == "__main__":
    unittest.main()
