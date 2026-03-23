from __future__ import annotations

import json
import sqlite3
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

REPO_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from bot.analytics.api_admin import _AnalyticsAdminMixin
from bot.dashboard.live.live import _REQUIRED_SCOPES
from bot.monitoring.monitoring import TwitchMonitoringMixin


class _CompatSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.execute(sql_text, tuple(params or ()))

    def executemany(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.executemany(sql_text, params or ())

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _ConnCtx:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._compat = _CompatSqliteConn(conn)

    def __enter__(self) -> _CompatSqliteConn:
        return self._compat

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _AdminHarness(_AnalyticsAdminMixin):
    def _require_v2_admin_api(self, _request):
        return None


class _MonitoringHarness(TwitchMonitoringMixin):
    pass


class AdminStreamersDedupTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.execute(
            """
            CREATE TABLE twitch_partners_all_state (
                twitch_login TEXT,
                twitch_user_id TEXT,
                discord_user_id TEXT,
                discord_display_name TEXT,
                created_at TEXT,
                archived_at TEXT,
                require_discord_link INTEGER,
                is_on_discord INTEGER,
                manual_partner_opt_out INTEGER,
                raid_bot_enabled INTEGER,
                silent_ban INTEGER,
                silent_raid INTEGER,
                is_monitored_only INTEGER,
                is_verified INTEGER,
                is_partner_active INTEGER,
                live_ping_enabled INTEGER,
                status TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_raid_auth (
                twitch_login TEXT,
                twitch_user_id TEXT,
                scopes TEXT,
                needs_reauth INTEGER DEFAULT 0,
                raid_enabled INTEGER DEFAULT 0,
                authorized_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_live_state (
                twitch_user_id TEXT PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                last_stream_id TEXT,
                last_started_at TEXT,
                last_title TEXT,
                last_game_id TEXT,
                last_discord_message_id TEXT,
                last_notified_at TEXT,
                is_live INTEGER DEFAULT 0,
                last_seen_at TEXT,
                last_game TEXT,
                last_viewer_count INTEGER DEFAULT 0,
                last_tracking_token TEXT,
                active_session_id INTEGER,
                had_deadlock_in_session INTEGER DEFAULT 0,
                last_deadlock_seen_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE streamer_plans (
                twitch_login TEXT,
                plan_name TEXT,
                promo_disabled INTEGER,
                promo_message TEXT,
                raid_boost_enabled INTEGER,
                notes TEXT,
                manual_plan_id TEXT,
                manual_plan_expires_at TEXT,
                manual_plan_notes TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_billing_subscriptions (
                customer_reference TEXT,
                plan_id TEXT,
                status TEXT,
                updated_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_stream_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                streamer_login TEXT,
                started_at TEXT,
                ended_at TEXT,
                stream_title TEXT,
                game_name TEXT,
                avg_viewers REAL,
                peak_viewers INTEGER,
                duration_seconds INTEGER,
                follower_delta INTEGER
            )
            """
        )

        self.conn.executemany(
            """
            INSERT INTO twitch_partners_all_state (
                twitch_login, twitch_user_id, discord_user_id, discord_display_name,
                created_at, archived_at, require_discord_link, is_on_discord,
                manual_partner_opt_out, raid_bot_enabled, silent_ban, silent_raid, is_monitored_only,
                is_verified, is_partner_active, live_ping_enabled, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            [
                (
                    "alpha",
                    "123",
                    None,
                    "Alpha Display",
                    "2026-03-01T12:00:00+00:00",
                    None,
                    0,
                    1,
                    0,
                    1,
                    0,
                    0,
                    0,
                    1,
                    1,
                    1,
                    "active",
                ),
                (
                    "beta",
                    "456",
                    None,
                    "Beta Display",
                    "2026-03-01T12:00:00+00:00",
                    None,
                    0,
                    1,
                    0,
                    1,
                    0,
                    0,
                    0,
                    1,
                    1,
                    1,
                    "active",
                ),
                (
                    "archived_only",
                    "999",
                    None,
                    "Archived Display",
                    "2026-02-01T12:00:00+00:00",
                    "2026-02-10T12:00:00+00:00",
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    1,
                    "archived",
                ),
            ],
        )
        full_scope_string = " ".join(_REQUIRED_SCOPES)
        self.conn.executemany(
            """
            INSERT INTO twitch_raid_auth (
                twitch_login, twitch_user_id, scopes, needs_reauth, raid_enabled, authorized_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            [
                ("alpha", "123", full_scope_string, 0, 1, "2026-03-15T10:00:00+00:00"),
                ("beta", "456", "chat:read chat:edit", 1, 0, "2026-03-15T11:00:00+00:00"),
                ("archived_only", "999", "chat:read chat:edit", 0, 0, "2026-03-15T12:00:00+00:00"),
            ],
        )
        self.conn.executemany(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_seen_at, last_started_at,
                last_viewer_count, active_session_id, last_game
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            [
                (
                    "123",
                    "alpha",
                    1,
                    "2026-03-15T15:39:01+00:00",
                    "2026-03-15T12:04:59+00:00",
                    99,
                    77,
                    "Deadlock",
                ),
                (
                    "alpha",
                    "alpha",
                    0,
                    "2026-01-31T20:49:50+00:00",
                    "2026-01-31T19:48:26+00:00",
                    1,
                    None,
                    "Deadlock",
                ),
                (
                    "456",
                    "beta",
                    0,
                    "2026-03-14T15:39:01+00:00",
                    "2026-03-14T12:04:59+00:00",
                    10,
                    None,
                    "Deadlock",
                ),
            ],
        )
        self.conn.commit()

    async def test_admin_streamer_list_deduplicates_legacy_live_state_rows(self) -> None:
        harness = _AdminHarness()
        request = SimpleNamespace(headers={}, match_info={}, query={})

        with patch("bot.analytics.api_admin.storage.transaction", return_value=_ConnCtx(self.conn)):
            response = await harness._api_admin_streamers(request)

        payload = json.loads(response.text)
        self.assertEqual(payload["count"], 2)
        self.assertEqual([item["login"] for item in payload["items"]], ["alpha", "beta"])
        alpha = payload["items"][0]
        self.assertTrue(alpha["isLive"])
        self.assertEqual(alpha["viewerCount"], 99)
        self.assertEqual(alpha["partnerStatus"], "active")
        self.assertEqual(alpha["oauthStatus"], "connected")

    async def test_admin_streamer_detail_prefers_real_twitch_user_id_row(self) -> None:
        harness = _AdminHarness()
        request = SimpleNamespace(headers={}, match_info={"login": "alpha"})

        with patch("bot.analytics.api_admin.storage.transaction", return_value=_ConnCtx(self.conn)):
            response = await harness._api_admin_streamer_detail(request)

        payload = json.loads(response.text)
        self.assertEqual(payload["login"], "alpha")
        self.assertTrue(payload["isLive"])
        self.assertEqual(payload["stats"]["viewerCount"], 99)
        self.assertEqual(payload["stats"]["lastGame"], "Deadlock")
        self.assertTrue(payload["settings"]["oauthConnected"])
        self.assertFalse(payload["settings"]["oauthNeedsReauth"])

    async def test_admin_streamer_list_can_include_archived_rows(self) -> None:
        harness = _AdminHarness()
        request = SimpleNamespace(headers={}, match_info={}, query={"view": "all"})

        with patch("bot.analytics.api_admin.storage.transaction", return_value=_ConnCtx(self.conn)):
            response = await harness._api_admin_streamers(request)

        payload = json.loads(response.text)
        self.assertEqual(payload["count"], 3)
        archived = next(item for item in payload["items"] if item["login"] == "archived_only")
        self.assertEqual(archived["partnerStatus"], "archived")
        self.assertEqual(archived["oauthStatus"], "partial")

    async def test_admin_oauth_scope_status_counts_all_authorized_rows(self) -> None:
        harness = _AdminHarness()
        request = SimpleNamespace(headers={}, match_info={})

        with patch("bot.analytics.api_admin.storage.transaction", return_value=_ConnCtx(self.conn)):
            response = await harness._api_admin_system_oauth_scopes(request)

        payload = json.loads(response.text)
        self.assertEqual(payload["summary"]["totalAuthorized"], 3)
        self.assertEqual(payload["summary"]["fullScopeCount"], 1)
        archived = next(item for item in payload["items"] if item["login"] == "archived_only")
        self.assertEqual(archived["partnerStatus"], "archived")
        self.assertEqual(archived["oauthStatus"], "partial")

    async def test_admin_streamer_list_ignores_blank_user_id_cross_matches(self) -> None:
        full_scope_string = " ".join(_REQUIRED_SCOPES)
        self.conn.executemany(
            """
            INSERT INTO twitch_partners_all_state (
                twitch_login, twitch_user_id, discord_user_id, discord_display_name,
                created_at, archived_at, require_discord_link, is_on_discord,
                manual_partner_opt_out, raid_bot_enabled, silent_ban, silent_raid, is_monitored_only,
                is_verified, is_partner_active, live_ping_enabled, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            [
                (
                    "blank_alpha",
                    "",
                    None,
                    "Blank Alpha",
                    "2026-03-05T12:00:00+00:00",
                    None,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    1,
                    1,
                    "active",
                ),
                (
                    "blank_beta",
                    "",
                    None,
                    "Blank Beta",
                    "2026-03-06T12:00:00+00:00",
                    None,
                    0,
                    1,
                    0,
                    1,
                    0,
                    0,
                    0,
                    1,
                    1,
                    1,
                    "active",
                ),
            ],
        )
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_seen_at, last_started_at,
                last_viewer_count, active_session_id, last_game
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "",
                "blank_beta",
                1,
                "2026-03-15T16:00:00+00:00",
                "2026-03-15T14:00:00+00:00",
                42,
                999,
                "Deadlock",
            ),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (
                twitch_login, twitch_user_id, scopes, needs_reauth, raid_enabled, authorized_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            ("blank_beta", "", full_scope_string, 0, 1, "2026-03-15T13:00:00+00:00"),
        )
        self.conn.commit()

        harness = _AdminHarness()
        request = SimpleNamespace(headers={}, match_info={}, query={"view": "all"})

        with patch("bot.analytics.api_admin.storage.transaction", return_value=_ConnCtx(self.conn)):
            response = await harness._api_admin_streamers(request)

        payload = json.loads(response.text)
        blank_alpha = next(item for item in payload["items"] if item["login"] == "blank_alpha")
        blank_beta = next(item for item in payload["items"] if item["login"] == "blank_beta")
        self.assertFalse(blank_alpha["isLive"])
        self.assertEqual(blank_alpha["oauthStatus"], "missing")
        self.assertTrue(blank_beta["isLive"])
        self.assertEqual(blank_beta["viewerCount"], 42)
        self.assertEqual(blank_beta["oauthStatus"], "connected")

    async def test_admin_oauth_scope_status_deduplicates_renamed_partner_matches(self) -> None:
        full_scope_string = " ".join(_REQUIRED_SCOPES)
        self.conn.executemany(
            """
            INSERT INTO twitch_partners_all_state (
                twitch_login, twitch_user_id, discord_user_id, discord_display_name,
                created_at, archived_at, require_discord_link, is_on_discord,
                manual_partner_opt_out, raid_bot_enabled, silent_ban, silent_raid, is_monitored_only,
                is_verified, is_partner_active, live_ping_enabled, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            [
                (
                    "renamed_new",
                    "222",
                    None,
                    "Renamed New",
                    "2026-03-10T12:00:00+00:00",
                    None,
                    0,
                    1,
                    0,
                    1,
                    0,
                    0,
                    0,
                    1,
                    1,
                    1,
                    "active",
                ),
                (
                    "renamed_old",
                    "222",
                    None,
                    "Renamed Old",
                    "2026-02-01T12:00:00+00:00",
                    "2026-03-09T12:00:00+00:00",
                    0,
                    1,
                    0,
                    0,
                    0,
                    0,
                    0,
                    1,
                    0,
                    1,
                    "archived",
                ),
            ],
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (
                twitch_login, twitch_user_id, scopes, needs_reauth, raid_enabled, authorized_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            ("renamed_new", "222", full_scope_string, 0, 1, "2026-03-15T13:30:00+00:00"),
        )
        self.conn.commit()

        harness = _AdminHarness()
        request = SimpleNamespace(headers={}, match_info={})

        with patch("bot.analytics.api_admin.storage.transaction", return_value=_ConnCtx(self.conn)):
            response = await harness._api_admin_system_oauth_scopes(request)

        payload = json.loads(response.text)
        renamed_items = [item for item in payload["items"] if item["login"] == "renamed_new"]
        self.assertEqual(payload["summary"]["totalAuthorized"], 4)
        self.assertEqual(payload["summary"]["fullScopeCount"], 2)
        self.assertEqual(len(renamed_items), 1)
        self.assertEqual(renamed_items[0]["partnerStatus"], "active")
        self.assertEqual(renamed_items[0]["oauthStatus"], "connected")

    async def test_admin_oauth_scope_status_uses_partner_login_for_blank_auth_login(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_partners_all_state (
                twitch_login, twitch_user_id, discord_user_id, discord_display_name,
                created_at, archived_at, require_discord_link, is_on_discord,
                manual_partner_opt_out, raid_bot_enabled, silent_ban, silent_raid, is_monitored_only,
                is_verified, is_partner_active, live_ping_enabled, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "shadow_login",
                "333",
                None,
                "Shadow Login",
                "2026-03-11T12:00:00+00:00",
                None,
                0,
                1,
                0,
                0,
                0,
                0,
                0,
                1,
                1,
                1,
                "active",
            ),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (
                twitch_login, twitch_user_id, scopes, needs_reauth, raid_enabled, authorized_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            ("", "333", "chat:read chat:edit", 0, 0, "2026-03-15T13:45:00+00:00"),
        )
        self.conn.commit()

        harness = _AdminHarness()
        request = SimpleNamespace(headers={}, match_info={})

        with patch("bot.analytics.api_admin.storage.transaction", return_value=_ConnCtx(self.conn)):
            response = await harness._api_admin_system_oauth_scopes(request)

        payload = json.loads(response.text)
        self.assertEqual(payload["summary"]["totalAuthorized"], 4)
        shadow = next(item for item in payload["items"] if item["login"] == "shadow_login")
        self.assertEqual(shadow["partnerStatus"], "active")
        self.assertEqual(shadow["oauthStatus"], "partial")

    async def test_monitoring_live_state_persist_removes_legacy_login_key_rows(self) -> None:
        self.conn.execute(
            """
            INSERT OR REPLACE INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_seen_at, last_started_at,
                last_viewer_count, active_session_id, last_game
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "gamma",
                "gamma",
                1,
                "2026-01-31T20:49:50+00:00",
                "2026-01-31T19:48:26+00:00",
                2,
                None,
                "Deadlock",
            ),
        )
        self.conn.commit()

        harness = _MonitoringHarness()
        rows = [
            (
                "789",
                "gamma",
                0,
                "2026-03-15T15:39:01+00:00",
                "Gamma Title",
                "Deadlock",
                8,
                None,
                None,
                "stream-789",
                "2026-03-15T12:04:59+00:00",
                1,
                None,
                "2026-03-15T15:39:01+00:00",
            )
        ]

        with patch("bot.monitoring.monitoring.storage.transaction", return_value=_ConnCtx(self.conn)):
            await harness._persist_live_state_rows(rows)

        live_rows = self.conn.execute(
            """
            SELECT twitch_user_id, streamer_login, is_live, last_viewer_count
            FROM twitch_live_state
            WHERE LOWER(streamer_login) = LOWER(?)
            ORDER BY twitch_user_id
            """,
            ("gamma",),
        ).fetchall()
        self.assertEqual(len(live_rows), 1)
        self.assertEqual(live_rows[0]["twitch_user_id"], "789")
        self.assertEqual(live_rows[0]["last_viewer_count"], 8)
