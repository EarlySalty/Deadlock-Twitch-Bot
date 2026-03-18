from __future__ import annotations

import sqlite3
import unittest
from unittest.mock import AsyncMock, patch

from bot.analytics.api_v2 import AnalyticsV2Mixin
from bot.chat.tokens import TokenPersistenceMixin


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


class _InternalHomeHarness(AnalyticsV2Mixin):
    def _load_internal_home_autoban_events(self, *, streamer_login: str, since_date: str):
        del streamer_login, since_date
        return []

    def _load_internal_home_service_warning_events(self, *, streamer_login: str, since_date: str):
        del streamer_login, since_date
        return []


class _DummyTokenManager:
    def __init__(self) -> None:
        self.access_token = None
        self.refresh_token = None
        self.bot_id = None
        self.expires_at = None
        self.scopes: set[str] = set()
        self._save_tokens = AsyncMock()


class _TokenHarness(TokenPersistenceMixin):
    def __init__(self) -> None:
        self._token_manager = _DummyTokenManager()


class InternalHomeRegressionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.execute(
            """
            CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                discord_user_id TEXT,
                discord_display_name TEXT,
                is_on_discord INTEGER DEFAULT 0
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                scopes TEXT NOT NULL,
                needs_reauth INTEGER DEFAULT 0,
                authorized_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_stream_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                streamer_login TEXT NOT NULL,
                started_at TEXT,
                ended_at TEXT,
                duration_seconds INTEGER DEFAULT 0,
                avg_viewers REAL DEFAULT 0,
                peak_viewers INTEGER DEFAULT 0,
                follower_delta INTEGER DEFAULT 0,
                followers_start INTEGER DEFAULT 0,
                followers_end INTEGER DEFAULT 0,
                stream_title TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_stats_tracked (
                ts_utc TEXT,
                streamer TEXT,
                viewer_count INTEGER
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_chat_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id INTEGER NOT NULL,
                streamer_login TEXT NOT NULL,
                chatter_login TEXT,
                chatter_id TEXT,
                message_id TEXT,
                message_ts TEXT NOT NULL,
                is_command INTEGER DEFAULT 0,
                content TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_ban_events (
                received_at TEXT,
                twitch_user_id TEXT,
                event_type TEXT,
                target_login TEXT,
                target_id TEXT,
                moderator_login TEXT,
                reason TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_raid_history (
                executed_at TEXT,
                from_broadcaster_id TEXT,
                from_broadcaster_login TEXT,
                to_broadcaster_id TEXT,
                to_broadcaster_login TEXT,
                viewer_count INTEGER,
                reason TEXT,
                success INTEGER
            )
            """
        )

    def tearDown(self) -> None:
        self.conn.close()

    def test_internal_home_uses_current_pg_column_names_for_stats_and_chat(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, is_on_discord)
            VALUES ('1001', 'new_login', 1)
            """
        )
        self.conn.execute(
            """
            INSERT INTO twitch_stream_sessions (
                streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers,
                follower_delta, followers_start, followers_end, stream_title
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "new_login",
                "2026-03-17T10:00:00+00:00",
                "2026-03-17T12:00:00+00:00",
                7200,
                42.0,
                64,
                5,
                100,
                105,
                "Morning Stream",
            ),
        )
        self.conn.executemany(
            "INSERT INTO twitch_stats_tracked (ts_utc, streamer, viewer_count) VALUES (?, ?, ?)",
            [
                ("2026-03-17T10:00:00+00:00", "new_login", 40),
                ("2026-03-17T10:15:00+00:00", "new_login", 44),
                ("2026-03-10T10:00:00+00:00", "new_login", 20),
            ],
        )
        self.conn.executemany(
            """
            INSERT INTO twitch_chat_messages (
                session_id, streamer_login, chatter_login, message_id, message_ts, content
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            [
                (1, "new_login", "viewer_a", "m1", "2026-03-17T10:30:00+00:00", "hello"),
                (1, "new_login", "viewer_b", "m2", "2026-03-17T11:15:00+00:00", "hi"),
            ],
        )
        handler = _InternalHomeHarness()

        with patch("bot.storage.pg.get_conn", return_value=_ConnCtx(self.conn)):
            payload = handler._build_internal_home_payload(
                twitch_login="new_login",
                twitch_user_id="1001",
                display_name="New Login",
                days=30,
            )

        self.assertEqual(payload["last_stream_summary"]["chat_messages"], 2)
        self.assertIsNotNone(payload["health_score"])
        self.assertIsNotNone(payload["week_comparison"])
        self.assertEqual(payload["health_score"]["sub_scores"]["retention"], 15)

    def test_internal_home_matches_oauth_by_user_id_and_surfaces_reauth(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, is_on_discord)
            VALUES ('1001', 'new_login', 0)
            """
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes, needs_reauth, authorized_at)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "old_login",
                "channel:manage:raids moderator:read:followers channel:read:subscriptions",
                1,
                "2026-03-17T09:00:00+00:00",
            ),
        )
        handler = _InternalHomeHarness()

        with patch("bot.storage.pg.get_conn", return_value=_ConnCtx(self.conn)):
            payload = handler._build_internal_home_payload(
                twitch_login="new_login",
                twitch_user_id="1001",
                display_name="New Login",
                days=30,
            )

        oauth = payload["status"]["oauth"]
        self.assertTrue(oauth["connected"])
        self.assertEqual(oauth["status"], "reauth")
        self.assertTrue(oauth["needs_reauth"])
        self.assertIn("channel:manage:raids", oauth["granted_scopes"])


class TokenPersistenceMixinTests(unittest.IsolatedAsyncioTestCase):
    async def test_persist_bot_tokens_updates_token_manager_scopes(self) -> None:
        harness = _TokenHarness()

        await harness._persist_bot_tokens(
            access_token="oauth:new-token",
            refresh_token="oauth:new-refresh",
            expires_in=3600,
            scopes=["channel:bot", "user:read:chat", "CHANNEL:BOT", ""],
            user_id="999",
        )

        self.assertEqual(
            harness._token_manager.scopes,
            {"channel:bot", "user:read:chat"},
        )
        harness._token_manager._save_tokens.assert_awaited_once()
