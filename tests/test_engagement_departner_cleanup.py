"""Departnern raeumt den Engagement-Layer mit auf.

Wird ein aktiver Partner departnert, darf die Engagement-AI dort nicht weiter
antworten. ``departner_active_partner`` setzt deshalb
``twitch_engagement_settings.enabled = FALSE``, analog zum bestehenden
raid-auth-Cleanup. Das Laufzeit-Gate in der Pipeline greift zusaetzlich; dieser
Cleanup haelt den Settings-Zustand sauber statt enabled=TRUE verwaisen zu lassen.
"""

from __future__ import annotations

import sqlite3
import unittest

from bot.storage.partner_registry import departner_active_partner


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
            admin_archived_at TEXT,
            departnered_at TEXT,
            technical_pause_reason TEXT,
            status TEXT NOT NULL DEFAULT 'active'
        )
        """
    )
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
        CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            raid_enabled INTEGER DEFAULT 1,
            needs_reauth INTEGER DEFAULT 0
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_engagement_settings (
            channel_login TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            irc_read INTEGER NOT NULL DEFAULT 0
        )
        """
    )
    return conn


class EngagementDepartnerCleanupTests(unittest.TestCase):
    def test_departner_disables_engagement_settings(self) -> None:
        conn = _make_conn()
        compat = _SqlitePgCompatConnection(conn)
        conn.execute(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, partnered_at) "
            "VALUES (?, ?, 'active', ?)",
            ("1001", "earlysalty", "2026-05-01T10:00:00+00:00"),
        )
        conn.execute(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES (?, ?)",
            ("1001", "earlysalty"),
        )
        conn.execute(
            "INSERT INTO twitch_engagement_settings (channel_login, enabled, irc_read) "
            "VALUES (?, 1, 1)",
            ("earlysalty",),
        )

        result = departner_active_partner(compat, twitch_user_id="1001")

        self.assertIsNotNone(result)
        # Partner ist wirklich departnert (Vorbedingung des Cleanups)
        status = conn.execute(
            "SELECT status FROM twitch_partners WHERE twitch_user_id = ?", ("1001",)
        ).fetchone()[0]
        self.assertNotEqual(status, "active")
        # Engagement-Settings wurden mit abgeschaltet
        enabled = conn.execute(
            "SELECT enabled FROM twitch_engagement_settings WHERE channel_login = ?",
            ("earlysalty",),
        ).fetchone()[0]
        self.assertEqual(enabled, 0)

    def test_departner_without_engagement_row_does_not_raise(self) -> None:
        """Kein Engagement-Eintrag fuer den Channel -> Cleanup ist ein No-op,
        Departnern laeuft normal durch."""
        conn = _make_conn()
        compat = _SqlitePgCompatConnection(conn)
        conn.execute(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, partnered_at) "
            "VALUES (?, ?, 'active', ?)",
            ("2002", "partnerbravo", "2026-05-01T10:00:00+00:00"),
        )
        conn.execute(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES (?, ?)",
            ("2002", "partnerbravo"),
        )

        result = departner_active_partner(compat, twitch_user_id="2002")

        self.assertIsNotNone(result)
        count = conn.execute("SELECT COUNT(*) FROM twitch_engagement_settings").fetchone()[0]
        self.assertEqual(count, 0)


if __name__ == "__main__":
    unittest.main()
